#[cfg(any(feature = "kyotocabinet", feature = "bdb"))]
#[test]
fn mask_all_empties_every_value_and_keeps_every_key() {
    use super::Tree;

    // `mask_out(0x0, 0x0)` removes every token of every record: the
    // Kyoto Cabinet visitor returns each record's now-empty chunk
    // (phrase_large_table3_kyotodb.cpp:361-369,
    // chewing_large_table2_kyotodb.cpp:459-479 at 074a2219) and the
    // Berkeley DB cursor puts that empty chunk back with `DB_CURRENT`
    // (phrase_large_table3_bdb.cpp:391-400). The records, including the
    // ones that were already empty, stay in the table.
    let table = Tree::memory().expect("in-memory table");
    let rows: [(&[u8], &[u8]); 3] = [
        (&[1, 0, 0, 0], &[7, 0, 0, 0, 8, 0, 0, 0]),
        (&[2, 0, 0, 0], &[9, 0, 0, 0]),
        (&[3, 0, 0, 0], &[]),
    ];
    for (key, value) in rows {
        table.put(key, value).expect("put");
    }
    table.mask_all();
    let mut keys = table.native_keys().expect("keys");
    keys.sort();
    assert_eq!(
        keys,
        rows.iter().map(|(k, _)| k.to_vec()).collect::<Vec<_>>()
    );
    for (key, _) in rows {
        assert_eq!(table.get(key), Some(Vec::new()), "{key:?}");
    }
}
