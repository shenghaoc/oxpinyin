use super::*;

/// 074a2219 phrase_large_table3_tkrzwdb.cpp:142 retains SEARCH_CONTINUED for
/// an existing prefix key even when its value has no complete tokens.
#[test]
fn an_empty_phrase_row_still_reports_continuation() {
    let table = Tree::memory().unwrap();
    table.put(&encode_ucs4_key_scalars(&[0x4f60]), &[]).unwrap();
    let tables = Tables {
        table,
        indices: core::array::from_fn(|_| None),
    };
    let (continued, tokens) = tables.phrase_search(&[0x4f60]);
    assert!(continued);
    assert!(tokens.iter().all(Vec::is_empty));
    assert!(!tables.phrase_search(&[0x597d]).0);
}

/// 074a2219 chewing_large_table2.h:113-116 replaces a null-token cursor
/// with the next token; it does not consume and discard that next token.
#[test]
fn a_zero_cursor_keeps_the_following_chewing_token() {
    use oxpinyin_user::data::row_format::pinyin_index::encode_item;
    let key = ChewingKey::from_pinyin("ni").unwrap();
    let mut bytes = Vec::new();
    for token in [0, 1, 2] {
        bytes.extend(encode_item(token, &[key]));
    }
    let table = Tree::memory().unwrap();
    table.put(&index_key(&[key]), &bytes).unwrap();
    let mut indices = core::array::from_fn(|_| None);
    indices[0] = Some(Index::default());
    let tables = Tables { table, indices };
    let (_, ranges) = tables.chewing_search(&[key]);
    assert_eq!(ranges[0].len(), 1);
    assert_eq!(ranges[0][0], 1..3);
    assert!(ranges[1..].iter().all(Vec::is_empty));
}
