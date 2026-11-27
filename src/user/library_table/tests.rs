use super::LibraryTable;
use crate::data::LibrarySet;

/// Basis: Rust contract (a table over library indexes 0 to 15, walked in ascending order).
#[test]
fn a_table_walks_its_present_libraries_in_ascending_order() {
    let mut table: LibraryTable<&str> =
        [(7, "user"), (1, "gb"), (15, "last")].into_iter().collect();
    assert_eq!(table.insert(1, "gb again"), Some("gb"));
    assert_eq!(table.len(), 3);
    assert_eq!(
        table.iter().collect::<Vec<_>>(),
        [(&1, &"gb again"), (&7, &"user"), (&15, &"last")]
    );
    assert_eq!(table.keys().copied().collect::<Vec<_>>(), [1, 7, 15]);
    assert_eq!(table.keys().next_back(), Some(&15));
    assert_eq!(table.remove(&7), Some("user"));
    assert!(!table.contains_key(&7));
    table.retain(|library, _| *library != 15);
    assert_eq!(table.into_iter().collect::<Vec<_>>(), [(1, "gb again")]);
}

/// Basis: `novel_types.h:43` (`PHRASE_INDEX_LIBRARY_COUNT` 16): an index of 16 or more names no slot.
#[test]
fn an_index_past_the_last_library_names_no_slot() {
    let mut table = LibraryTable::new();
    assert_eq!(table.insert(16, 1_u32), None);
    assert_eq!(table.get(&16), None);
    assert_eq!(table.remove(&0x81), None);
    assert!(table.get_or_default(16).is_none());
    assert!(table.get_or_insert_with(0x81, || 2).is_none());
    assert!(table.is_empty());
    *table.get_or_default(15).expect("the last slot") += 3;
    assert_eq!(table.get(&15), Some(&3));

    let mut set = LibrarySet::new();
    assert!(!set.insert(16) && !set.contains(&16));
    assert!(set.insert(3) && !set.insert(3));
    assert_eq!(set.iter().collect::<Vec<_>>(), [3]);
    assert!(set.remove(&3) && set.is_empty());
}
