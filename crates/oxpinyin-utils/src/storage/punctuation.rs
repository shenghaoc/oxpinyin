//! The utilities' shared pin punctuation append operation.
use super::dbm::Tree;
use oxpinyin_input::tool;
use std::ffi::CStr;

/// `PunctTable::append_punctuation` / `PunctTableEntry::append_punctuation`
/// (`src/storage/punct_table.cpp:97-115`, `:158-167`).
pub(crate) fn append_punctuation(table: &Tree, token: u32, punctuation: &CStr) {
    let key = token.to_le_bytes();
    let (ucs4, _written) = tool::utf8_to_ucs4(punctuation);
    let ucs4 = ucs4.unwrap_or_default();
    let existing = table.get(&key).unwrap_or_default();
    // `get_all_punctuations`: zero-terminated UCS-4 runs.
    let mut runs: Vec<Vec<u32>> = Vec::new();
    let mut current = Vec::new();
    for word in existing
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
    {
        if word == 0 {
            runs.push(std::mem::take(&mut current));
        } else {
            current.push(word);
        }
    }
    if runs.contains(&ucs4) {
        tool::storage::report_duplicated_punctuation(punctuation);
        return;
    }
    let mut value = existing;
    for word in &ucs4 {
        value.extend_from_slice(&word.to_le_bytes());
    }
    value.extend_from_slice(&0_u32.to_le_bytes());
    let _ = table.put(&key, &value);
}

#[cfg(test)]
mod native_tests {
    use super::*;
    use oxpinyin_data::{
        PunctTable,
        row_format::punct::{decode_puncts, encode_puncts},
    };
    use oxpinyin_store::{DefaultStore, RawReadStore, WriteStore};
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
}
