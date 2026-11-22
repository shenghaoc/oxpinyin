use super::*;

use crate::parser::ChewingKey;

// Pinned vectors: a change to the shared implementation breaks both
// halves' tests at once, the same guarantee `chunk_format` carries.

#[test]
fn token_key_is_the_little_endian_word() {
    assert_eq!(encode_token_key(0x0100_0295), [0x95, 0x02, 0x00, 0x01]);
}

#[test]
fn item2_stride_matches_upstream_sizeof() {
    use pinyin_index::item2_stride;
    assert_eq!(item2_stride(1), 8, "L=1: 4+2=6 padded to 8");
    assert_eq!(item2_stride(2), 8, "L=2: 4+4=8 no padding");
    assert_eq!(item2_stride(3), 12, "L=3: 4+6=10 padded to 12");
    assert_eq!(item2_stride(4), 12, "L=4: 4+8=12 no padding");
    assert_eq!(item2_stride(5), 16, "L=5: 4+10=14 padded to 16");
    assert_eq!(item2_stride(6), 16, "L=6: 4+12=16 no padding");
    assert_eq!(item2_stride(16), 36, "L=16: max libpinyin phrase length");
}

#[test]
fn encode_complete_key_zeroes_tone() {
    let keys = [ChewingKey::new(1, 0, 2, 3)];
    let encoded = pinyin_index::encode_complete_key(&keys);
    assert_eq!(encoded.len(), 2);
    let expected = ChewingKey::new(1, 0, 2, 0).to_packed().to_le_bytes();
    assert_eq!(encoded, expected);
}

#[test]
fn encode_incomplete_key_keeps_only_initial() {
    let keys = [ChewingKey::new(5, 1, 3, 2)];
    let encoded = pinyin_index::encode_incomplete_key(&keys);
    assert_eq!(encoded.len(), 2);
    let expected = ChewingKey::new(5, 0, 0, 0).to_packed().to_le_bytes();
    assert_eq!(encoded, expected);
}

#[test]
fn encode_multi_syllable_key() {
    let keys = [ChewingKey::new(1, 0, 2, 0), ChewingKey::new(3, 1, 4, 0)];
    let encoded = pinyin_index::encode_complete_key(&keys);
    assert_eq!(encoded.len(), 4);
    let k0 = ChewingKey::new(1, 0, 2, 0).to_packed().to_le_bytes();
    let k1 = ChewingKey::new(3, 1, 4, 0).to_packed().to_le_bytes();
    assert_eq!(&encoded[0..2], &k0);
    assert_eq!(&encoded[2..4], &k1);
}

#[test]
fn round_trip_multi_key_item_and_multi_record_value() {
    use pinyin_index::{PinyinIndexItem, decode_items, encode_items, item2_stride};
    let ni_hao = PinyinIndexItem {
        token: 0x0100_0099,
        keys: vec![
            ChewingKey::new(14, 0, 7, 3), // ni3
            ChewingKey::new(8, 0, 2, 3),  // hao3
        ],
    };
    let encoded = encode_items(std::slice::from_ref(&ni_hao));
    assert_eq!(encoded.len(), item2_stride(2));
    assert_eq!(decode_items(&encoded, 2).unwrap(), vec![ni_hao]);

    // Two L=1 records in one value: each lands at its own stride.
    let items = vec![
        PinyinIndexItem {
            token: 0x0100_0001,
            keys: vec![ChewingKey::new(1, 0, 2, 1)],
        },
        PinyinIndexItem {
            token: 0x0100_0002,
            keys: vec![ChewingKey::new(1, 0, 2, 3)],
        },
    ];
    let encoded = encode_items(&items);
    assert_eq!(encoded.len(), item2_stride(1) * 2);
    assert_eq!(decode_items(&encoded, 1).unwrap(), items);
}

/// The bytes `encode_item` writes, pinned: the token as a
/// little-endian word, then each key as its packed `u16`
/// (`initial | middle << 5 | final << 7 | tone << 12`) in
/// little-endian order, then the stride's zero padding. A change to
/// the word or key byte order fails here.
#[test]
fn item_bytes_are_the_little_endian_token_then_the_packed_keys() {
    use pinyin_index::encode_item;
    // ni3: 14 | 7 << 7 | 3 << 12 = 0x338E.
    let ni3 = ChewingKey::new(14, 0, 7, 3);
    // hao3: 8 | 2 << 7 | 3 << 12 = 0x3108.
    let hao3 = ChewingKey::new(8, 0, 2, 3);
    assert_eq!(
        encode_item(0x0100_0042, &[ni3]),
        [0x42, 0x00, 0x00, 0x01, 0x8E, 0x33, 0x00, 0x00]
    );
    assert_eq!(
        encode_item(0x0100_0099, &[ni3, hao3]),
        [0x99, 0x00, 0x00, 0x01, 0x8E, 0x33, 0x08, 0x31]
    );
}

#[test]
fn decode_items_drops_a_partial_record_and_rejects_zero_length() {
    use pinyin_index::{decode_items, encode_item};
    let ni3 = ChewingKey::new(1, 0, 2, 3);
    // An 8-byte stride (one key) and a 12-byte stride (three keys): the
    // pin's pointer difference is exact for the first and undefined for
    // the second (row 128); both keep the whole records.
    for keys in [vec![ni3], vec![ni3; 3]] {
        let mut row = encode_item(0x0100_0001, &keys);
        row.extend_from_slice(&encode_item(0x0100_0002, &keys)[..4]);
        let items = decode_items(&row, keys.len()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].token, 0x0100_0001);
        assert_eq!(items[0].keys, keys);
    }
    assert!(decode_items(&[0; 7], 1).unwrap().is_empty());
    assert!(decode_items(&[0; 8], 0).is_err());
    assert!(decode_items(&[], 1).unwrap().is_empty());
}

#[test]
fn ucs4_key_round_trip() {
    use phrase_index::{encode_ucs4_key, encode_ucs4_key_scalars};
    let key = encode_ucs4_key("你好");
    assert_eq!(key.len(), 8);
    assert_eq!(
        u32::from_le_bytes(key[0..4].try_into().unwrap()),
        0x4F60,
        "你 = U+4F60"
    );
    // The two entry points agree byte for byte.
    assert_eq!(key, encode_ucs4_key_scalars(&[0x4F60, 0x597D]));
}

#[test]
fn phrase_tokens_round_trip() {
    use phrase_index::{decode_tokens, test_encode_tokens as encode_tokens};
    let tokens = vec![0x0100_0001, 0x0200_0042];
    let encoded = encode_tokens(&tokens);
    assert_eq!(encoded.len(), 8);
    assert_eq!(decode_tokens(&encoded).unwrap(), tokens);
    assert!(decode_tokens(&[]).unwrap().is_empty(), "prefix marker");
    assert!(decode_tokens(&[0, 0, 0]).is_err());
}

#[test]
fn punct_stream_reads_zero_terminated_words() {
    assert_eq!(
        punct::decode_puncts(&words(&[0xff0c, 0, 0x3002, 0])),
        ["，", "。"]
    );
    assert!(punct::decode_puncts(b"").is_empty());
}

fn words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

/// `unescape` appends the first word unconditionally, so a zero there
/// is an empty punctuation whose run still takes the next nonzero
/// words (`punct_table.cpp:64-69`).
#[test]
fn punct_stream_keeps_empty_fields() {
    use punct::decode_puncts;
    assert_eq!(decode_puncts(&words(&[0, 0])), [""]);
    assert_eq!(
        decode_puncts(&words(&[0, 0xFF0C, 0, 0x3002, 0])),
        ["", "。"]
    );
    assert_eq!(decode_puncts(&words(&[0xFF0C, 0, 0, 0])), ["，", ""]);
    assert_eq!(decode_puncts(&words(&[0xFF0C, 0, 0])), ["，", ""]);
}

/// The last run ends at the value's end whether or not a zero follows
/// it (`punct_table.cpp:64-71`).
#[test]
fn punct_stream_ends_an_unterminated_run() {
    use punct::decode_puncts;
    assert_eq!(decode_puncts(&words(&[0xFF0C])), ["，"]);
    assert_eq!(
        decode_puncts(&words(&[0xFF0C, 0, 0x3002, 0x3001])),
        ["，", "。、"]
    );
}

/// Partial words cannot add punctuation entries.
#[test]
fn punct_stream_ignores_partial_words() {
    use punct::decode_puncts;
    assert!(decode_puncts(b"\x00").is_empty());
    assert!(decode_puncts("，".as_bytes()).is_empty());
    let mut value = words(&[0xFF0C, 0]);
    value.push(0x41);
    assert_eq!(decode_puncts(&value), ["，"]);
    let mut value = words(&[0xFF0C]);
    value.extend_from_slice(&[0x41, 0x42]);
    assert_eq!(decode_puncts(&value), ["，"]);
}

/// Non-scalar words are kept as stored words, not refused.
#[test]
fn punct_stream_keeps_non_scalar_words() {
    use punct::decode_puncts;
    let decoded = decode_puncts(&words(&[0xD800, 0, 0x11_0000, 0xFF0C, 0, 0xFFFF_FFFF, 0]));
    let stored: Vec<_> = decoded.iter().map(|text| text.stored_words()).collect();
    assert_eq!(
        stored,
        [
            Some(&[0xD800][..]),
            Some(&[0x11_0000, 0xFF0C][..]),
            Some(&[0xFFFF_FFFF][..])
        ]
    );
}
