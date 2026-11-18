use super::*;
use oxpinyin::store::{DefaultStore, RawReadStore, WriteStore};
use oxpinyin::user::data::{
    PunctTable,
    row_format::punct::{decode_puncts, encode_puncts},
};
use std::ops::Bound;

#[test]
fn punctuation_keeps_its_order_in_a_snapshot_and_the_empty_row_is_readable() {
    // tests/storage/test_punct_table.cpp:47-66 at 074a2219: attach,
    // three appends, save_db and load_db. Its `remove_punctuation`
    // calls (`:59`, `:68`, `:72`) have no product counterpart and are
    // not simulated here.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("punct.bin");
    let snapshot = dir.path().join("snapshot.db");
    let token = 1_u32;
    let key = token.to_le_bytes();
    // Include the empty-file attach case before appending.
    std::fs::write(&path, []).unwrap();
    let tree = Tree::create(&path).unwrap();
    for punctuation in [c"……", c"…", c"？"] {
        append_punctuation(&tree, token, punctuation);
    }
    assert_eq!(tree.get(&key), Some(encode_puncts(&["……", "…", "？"])));
    drop(tree);

    let source = DefaultStore::open_read_only(&path).unwrap();
    DefaultStore::write_user_index(&snapshot, |sink| {
        source.range_raw(Bound::Unbounded, Bound::Unbounded, sink)
    })
    .unwrap();
    let occupied = dir.path().join("occupied");
    std::fs::create_dir(&occupied).unwrap();
    std::fs::write(occupied.join("keep"), b"kept").unwrap();
    assert!(Tree::create(&occupied).is_err());
    assert!(
        DefaultStore::write_user_index(&occupied, |sink| {
            source.range_raw(Bound::Unbounded, Bound::Unbounded, sink)
        })
        .is_err()
    );
    assert_eq!(std::fs::read(occupied.join("keep")).unwrap(), b"kept");

    // Kyoto Cabinet loads snapshot bytes through its snapshot API,
    // while the product's punctuation table opens a TreeDB file.
    let copy = DefaultStore::open_user_index(&snapshot).unwrap();
    let mut entry = Vec::new();
    assert!(copy.get_raw(&key, &mut entry).unwrap());
    assert_eq!(decode_puncts(&entry), ["……", "…", "？"]);
    let table = PunctTable::open(&path).unwrap();
    assert_eq!(table.punctuations(token).unwrap(), ["……", "…", "？"]);
    drop(table);
    drop(source);
    // A hand-written zero-byte row, not a removal: the reader accepts it.
    let tree = Tree::create(&path).unwrap();
    tree.put(&key, &[]).unwrap();
    assert_eq!(tree.get(&key), Some(Vec::new()));
    drop(tree);
    let table = PunctTable::open(&path).unwrap();
    assert!(table.punctuations(token).unwrap().is_empty());
}
