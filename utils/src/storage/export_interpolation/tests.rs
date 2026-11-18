use std::collections::BTreeMap;

use oxpinyin::data::chunk_write::ChunkItem;
use oxpinyin::phrase_index_make_token;

use super::walked_items;
use crate::storage::libraries::Library;

/// A library with items at `slots` and the trimmed offset count
/// `range_end`.
fn library(slots: &[u32], range_end: u32) -> Library {
    Library {
        name: c"gbk.bin".to_owned(),
        items: slots
            .iter()
            .map(|&slot| {
                (
                    slot,
                    ChunkItem {
                        phrase: vec![0x4e00 + slot],
                        unigram: slot,
                        prons: Vec::new(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
        range_end,
        total: 0,
    }
}

/// `gen_unigram` at 074a2219 (`utils/storage/export_interpolation.cpp:85-90`)
/// walks the tokens below `get_range`'s end, which
/// `PHRASE_INDEX_MAKE_TOKEN` cuts to 24 bits (`src/storage/phrase_index.cpp:619-620`).
/// Of a sparse library, it reads only the items below that end, in slot
/// order; items at or past the end, and slot 0, are not read.
#[test]
fn a_sparse_library_yields_only_its_items_below_the_masked_end() {
    let slots = [0, 1, 4, 9, 12, 30];
    for range_end in [12, (1 << 24) + 12] {
        let walked: Vec<(u32, u32)> = walked_items(2, &library(&slots, range_end))
            .map(|(token, item)| (token, item.unigram))
            .collect();
        assert_eq!(
            walked,
            [1, 4, 9].map(|slot| (phrase_index_make_token(2, slot), slot)),
            "range end {range_end:#x}"
        );
    }
    // A masked end of 0 or 1 reads nothing.
    for range_end in [0, 1, 1 << 24, (1 << 24) + 1] {
        assert_eq!(
            walked_items(2, &library(&slots, range_end)).count(),
            0,
            "range end {range_end:#x}"
        );
    }
}
