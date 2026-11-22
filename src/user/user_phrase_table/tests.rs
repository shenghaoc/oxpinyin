use super::*;

#[test]
fn add_remove_and_prefix_markers_match_the_entry_ops() {
    let mut table = UserPhraseTable::new();
    let phrase = [0x4f60, 0x597d];
    let key = key_ucs4(&phrase);
    let stored_token = 9_u32.to_le_bytes();
    table.add(&[0x4f60], 7).unwrap();
    table.add(&phrase, 9).unwrap();
    assert_eq!(
        table.record(&key_ucs4(&[0x4f60])).as_deref(),
        Some(&[7, 0, 0, 0][..])
    );
    assert_eq!(table.record(&key).as_deref(), Some(stored_token.as_slice()));

    table.add(&phrase, 9).unwrap();
    assert_eq!(table.record(&key).as_deref(), Some(stored_token.as_slice()));

    table.remove(&phrase, 8).unwrap();
    assert_eq!(table.record(&key).as_deref(), Some(stored_token.as_slice()));

    table.remove(&[0x597d], 9).unwrap();
    assert_eq!(table.record(&key_ucs4(&[0x597d])), None);

    table.remove(&phrase, 9).unwrap();
    assert_eq!(table.record(&key).as_deref(), Some([].as_slice()));
}

#[test]
fn dense_mask_keeps_duplicate_tokens_in_stored_order() {
    let tokens = [0_u32, 5, 2, 4, 1, 6, 5, 8];
    let mut entry =
        PhraseTableEntry::from_bytes(tokens.into_iter().flat_map(u32::to_le_bytes).collect())
            .unwrap();
    assert!(entry.mask_out(1, 0));
    assert_eq!(entry.as_bytes(), [5, 0, 0, 0, 1, 0, 0, 0, 5, 0, 0, 0]);
}
