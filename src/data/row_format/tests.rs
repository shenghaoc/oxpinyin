use super::*;

use crate::parser::ChewingKey;

// Rust encode_token_key writes a little-endian u32.
#[test]
fn token_key_is_the_little_endian_word() {
    assert_eq!(encode_token_key(0x0100_0295), [0x95, 0x02, 0x00, 0x01]);
}

// 074a2219 chewing_large_table2.h:91-133 aligns IndexItem to a token boundary.
#[test]
fn item2_stride_matches_upstream_sizeof() {
    use pinyin_index::item2_stride;
    assert_eq!(item2_stride(1), 8, "L=1: 4+2=6 padded to 8");
    assert_eq!(item2_stride(2), 8, "L=2: 4+4=8 no padding");
}

// Rust index encoders keep key order and discard fields outside their keyspace.
#[test]
fn index_keys_encode_the_selected_keyspace() {
    use pinyin_index::{encode_complete_key, encode_incomplete_key};
    let keys = [ChewingKey::new(1, 0, 2, 3), ChewingKey::new(3, 1, 4, 0)];
    for (encoded, expected) in [
        (
            encode_complete_key(&keys),
            [ChewingKey::new(1, 0, 2, 0), ChewingKey::new(3, 1, 4, 0)],
        ),
        (
            encode_incomplete_key(&keys),
            [ChewingKey::new(1, 0, 0, 0), ChewingKey::new(3, 0, 0, 0)],
        ),
    ] {
        assert_eq!(
            encoded,
            expected.map(|key| key.to_packed().to_le_bytes()).concat()
        );
    }
}

// 074a2219 chewing_large_table2.h:145-150 addresses records at their stored stride.
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

// Rust encode_item writes the token, packed keys and zero padding in little-endian order.
#[test]
fn item_bytes_are_the_little_endian_token_then_the_packed_keys() {
    use pinyin_index::encode_item;
    let ni3 = ChewingKey::new(14, 0, 7, 3);
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

// Row 128 keeps whole records for both defined and class-(b) strides.
#[test]
fn decode_items_drops_a_partial_record_and_rejects_zero_length() {
    use pinyin_index::{decode_items, encode_item};
    let ni3 = ChewingKey::new(1, 0, 2, 3);
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

// Rust phrase index keys encode UCS-4 words in little-endian order.
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
    assert_eq!(key, encode_ucs4_key_scalars(&[0x4F60, 0x597D]));
}

// Rust decode_tokens accepts only a whole number of four-byte tokens, including zero.
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

// Rows 141 and 153 end the last punctuation run at the last whole word.
#[test]
fn punct_stream_decodes_whole_words() {
    for (run, tail, expected) in [
        (&[0xff0c, 0, 0x3002, 0][..], &[][..], &["，", "。"][..]),
        (&[], &[], &[]),
        (&[0, 0xff0c, 0, 0x3002, 0], &[], &["", "。"]),
        (&[0xff0c, 0, 0], &[], &["，", ""]),
        (&[0xff0c, 0, 0x3002, 0x3001], &[], &["，", "。、"]),
        (&[0xff0c, 0], &[0x41], &["，"]),
        (&[0xff0c], &[0x41, 0x42], &["，"]),
    ] {
        let mut value = words(run);
        value.extend_from_slice(tail);
        assert_eq!(punct::decode_puncts(&value), expected);
    }
}

fn words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

// Row 153 retains non-scalar punctuation words in stored form.
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
