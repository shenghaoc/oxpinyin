use super::*;
use oxpinyin::data::chunk_write::ChunkItem;

/// tests/storage/test_phrase_index_logger.cpp:32-38 at 074a2219: one
/// `add_unigram_frequency` per slot of the range, then the facade total.
#[test]
fn unigram_generation_counts_holes_and_refused_adds_in_the_facade_only() {
    for (total, facade_total, subindex_total, counts) in
        [(7_u32, 10, 9, [3, 6]), (u32::MAX, 2, u32::MAX, [2, 5])]
    {
        let mut by_index: [Option<Library>; 16] = Default::default();
        by_index[1] = Some(Library {
            name: c"gb.bin".to_owned(),
            items: BTreeMap::from([
                (
                    1,
                    ChunkItem {
                        phrase: vec![97],
                        unigram: 2,
                        prons: Vec::new(),
                    },
                ),
                (
                    3,
                    ChunkItem {
                        phrase: vec![99],
                        unigram: 5,
                        prons: Vec::new(),
                    },
                ),
            ]),
            total,
        });
        let mut libraries = Libraries { by_index, total };
        libraries.add_unigram_to_every_item();
        assert_eq!(libraries.total_freq(), facade_total);
        let library = libraries.by_library(1).unwrap();
        assert_eq!(library.total, subindex_total);
        assert_eq!(library.items.keys().copied().collect::<Vec<_>>(), [1, 3]);
        assert_eq!(
            library
                .items
                .values()
                .map(|item| item.unigram)
                .collect::<Vec<_>>(),
            counts
        );
    }
}
