use crate::data::phrase_library::ChunkItem;
use crate::parser::ChewingKey;

use crate::user::replay::UnigramAdd;
use crate::user::{SystemLibrary, SystemOriginals};

use super::SystemItems;

/// Library 1 holding three shipped items at slots 1-3, total 180.
fn three_item_originals() -> SystemOriginals {
    let ni = ChewingKey::new(23, 0, 1, 0).to_packed();
    let make = |word: u32, unigram: u32, frequency: u32| ChunkItem {
        phrase: vec![word],
        unigram,
        prons: vec![(vec![ni], frequency)],
    };
    crate::user::LibraryTable::from([(
        1_u8,
        SystemLibrary {
            mapped: None,
            total: 180,
            range_end: 4,
            items: [
                make(0x4f60, 100, 100),
                make(0x597d, 50, 50),
                make(0x4e16, 30, 30),
            ]
            .into_iter()
            .zip(1_u32..)
            .map(|(item, slot)| (slot, item))
            .collect(),
        },
    )])
    .into()
}

/// Basis: `phrase_index.cpp:150-176` (a refused add moves no sub-index total; a reload keeps the saved one).
#[test]
fn training_creates_a_live_buffer_with_the_saved_library_total() {
    let originals = three_item_originals();
    let original = originals.get(&1).expect("original");
    let token = crate::vocab::phrase_index_make_token(1, 1);
    let saved_total = u32::MAX - 3;
    let mut items =
        SystemItems::default().with_totals(crate::user::LibraryTable::from([(1, saved_total)]), 0);
    assert_eq!(items.library_total(1, Some(original)), Some(saved_total));
    assert!(items.train_live(
        Some(original),
        token,
        &mut crate::model::lent(std::iter::once(vec![ChewingKey::new(23, 0, 1, 0)])),
        1
    ));
    assert_eq!(items.library_total(1, Some(original)), Some(saved_total));
    assert_eq!(
        items.add_unigram(Some(original), token, 4),
        Some(UnigramAdd::Refused)
    );
}
