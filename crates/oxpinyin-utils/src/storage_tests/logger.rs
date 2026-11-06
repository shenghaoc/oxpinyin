//! tests/storage/test_phrase_index_logger.cpp:26-65 at 074a2219.
use super::*;
use oxpinyin_data::chunk_format::build_memory_chunk;
use oxpinyin_input::tool::phrase_tests as output;

fn load() -> PhraseLibrary {
    // Maintainer-approved class (b): the ignored false would enter
    // SubPhraseIndex::load through a NULL buffer (`phrase_index.cpp:354`).
    let payload =
        payload(Path::new("../../data/gb_char.bin")).unwrap_or_else(|| tool::exit(tool::ENOENT));
    library(payload)
}

fn load_index() -> (Index, bool) {
    let library = load();
    let accepted = !library.is_layout_failed();
    (Index::from_library(library), accepted)
}

/// Diff, persist and merge the actual native index, including facade range holes.
pub fn main() {
    let (mut index, mut accepted) = load_index();
    let mut facade_total = if accepted { index.total() } else { 0 };
    let delta = 1;
    for slot in index.range() {
        facade_total = facade_total.wrapping_add(delta);
        index.add_unigram(slot, delta);
    }
    output::total(facade_total);
    save(
        crate::fixed_tmp::path("gb_char.bin").as_path(),
        &stored(&index),
    );
    let log = present(index.diff(load()));
    save(
        crate::fixed_tmp::path("gb_char.dbin").as_path(),
        &present(build_memory_chunk(&log).ok()),
    );
    // FacadePhraseIndex::load keeps the facade cache and adjusts it by
    // the old total unconditionally and the new one only on accepted load
    // (`phrase_index.cpp:243-248`).
    let old_total = index.total();
    facade_total = facade_total.wrapping_sub(old_total);
    (index, accepted) = load_index();
    if accepted {
        facade_total = facade_total.wrapping_add(index.total());
    }
    let log = payload(crate::fixed_tmp::path("gb_char.dbin").as_path()).unwrap_or_default();
    let old_total = index.total();
    check(index.merge(&log).unwrap_or_else(|_| tool::abort()));
    // Merge adjusts the same cache, independently of its item fields
    // (`phrase_index.cpp:291-296`). Holes and refused adds stay in it.
    facade_total = facade_total
        .wrapping_sub(old_total)
        .wrapping_add(index.total());
    save(
        crate::fixed_tmp::path("gb_char2.bin").as_path(),
        &stored(&index),
    );
    output::total(facade_total);
}
