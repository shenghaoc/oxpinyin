use super::*;

fn key(initial: u8, middle: u8, final_: u8) -> ChewingKey {
    ChewingKey::new(initial, middle, final_, 0)
}

fn row(token: u32, keys: &[ChewingKey]) -> PinyinIndexItem {
    PinyinIndexItem {
        token,
        keys: keys.to_vec(),
    }
}

#[test]
fn pinyin_index_two_spaces_with_markers() {
    // One two-syllable row: "zhong guo" shape — distinct elements.
    let rows = vec![row(0x0100_0001, &[key(23, 0, 1), key(7, 0, 13)])];
    let entries = pinyin_index_entries(&rows);
    // Both spaces: lengths 1 (marker) and 2 (real) → 4 keys.
    assert_eq!(entries.len(), 4);
    // Incomplete length-1 key: initial-only first syllable.
    let inc1 = encode_incomplete_key(&[key(23, 0, 0)]);
    let inc2 = encode_incomplete_key(&[key(23, 0, 0), key(7, 0, 0)]);
    let comp2 = encode_complete_key(&[key(23, 0, 1), key(7, 0, 13)]);
    let by_key: BTreeMap<Vec<u8>, Vec<u8>> = entries.into_iter().collect();
    assert!(by_key[&inc1].is_empty(), "length-1 key is a marker");
    assert_eq!(by_key[&inc2].len(), 8, "L=2 records stride 8");
    assert_eq!(by_key[&comp2].len(), 8);
    // The complete-space length-1 prefix is the tone-zeroed first
    // syllable — a marker unless another row stores it.
    let comp1 = encode_complete_key(&[key(23, 0, 1)]);
    assert!(by_key[&comp1].is_empty());
}

#[test]
fn pinyin_index_records_sort_by_exact_compare2_then_token() {
    // Two rows sharing a complete key: record order is exact_compare2
    // (all equal here) then token ascending.
    let rows = vec![
        row(0x0100_0009, &[key(5, 0, 3)]),
        row(0x0100_0002, &[key(5, 0, 3)]),
    ];
    let entries = pinyin_index_entries(&rows);
    let full_key = encode_complete_key(&[key(5, 0, 3)]);
    let (_, value) = entries.iter().find(|(k, _)| *k == full_key).expect("key");
    assert_eq!(value.len(), 16, "two L=1 records at stride 8");
    let first = u32::from_le_bytes(value[0..4].try_into().unwrap());
    let second = u32::from_le_bytes(value[8..12].try_into().unwrap());
    assert_eq!((first, second), (0x0100_0002, 0x0100_0009));
}

#[test]
fn phrase_index_tokens_ascending_with_prefix_markers() {
    // 你好 (two chars) with two tokens inserted out of order, plus a
    // row reusing the first character as a full phrase.
    let rows = vec![
        (vec![0x4f60, 0x597d], 0x0100_0009_u32),
        (vec![0x4f60, 0x597d], 0x0100_0002),
        (vec![0x4f60, 0x597d], 0x0100_0002), // duplicate token: no-op
        (vec![0x4f60], 0x0100_0005),
    ];
    let entries = phrase_index_entries(&rows);
    let by_key: BTreeMap<Vec<u8>, Vec<u8>> = entries.into_iter().collect();
    let ni: Vec<u8> = 0x4f60_u32.to_le_bytes().to_vec();
    let nihao: Vec<u8> = [0x4f60_u32, 0x597d]
        .iter()
        .flat_map(|c| c.to_le_bytes())
        .collect();
    // 你 is both a prefix marker target and a real phrase: real value.
    assert_eq!(by_key[&ni], 0x0100_0005_u32.to_le_bytes().to_vec());
    assert_eq!(by_key[&nihao].len(), 8);
    let t0 = u32::from_le_bytes(by_key[&nihao][0..4].try_into().unwrap());
    let t1 = u32::from_le_bytes(by_key[&nihao][4..8].try_into().unwrap());
    assert_eq!((t0, t1), (0x0100_0002, 0x0100_0009));
    assert_eq!(by_key.len(), 2);
}
