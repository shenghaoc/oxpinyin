use super::load_heap;
use oxpinyin::data::phrase_library::{ChunkItem, build_chunk};

#[test]
fn heap_loaded_library_keeps_its_payload_after_file_changes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("library.bin");
    let item = ChunkItem {
        phrase: vec![0x4f60],
        unigram: 7,
        prons: vec![],
    };
    let bytes = build_chunk(&[(1, &item)]).expect("chunk");
    std::fs::write(&path, bytes).expect("write");
    let library = load_heap(&path).expect("load");
    std::fs::write(&path, []).expect("replace file");
    assert_eq!(library.item(1).map(|item| item.unigram()), Some(7));
    assert_eq!(library.total_freq(), 7);
}
