use super::*;
use oxpinyin_data::phrase_library::PhraseLibrary;

/// tests/storage/test_phrase_index.cpp:81-120 at 074a2219, through the
/// utility's own `load_phrase_index`: the 14-character item, the
/// two-reading item and the unigram delta of 3.
#[test]
fn the_native_chunk_keeps_long_text_and_same_token_readings() {
    let dir = tempfile::tempdir().unwrap();
    let table = dir.path().join("gb.table");
    let chunk = dir.path().join("gb.bin");
    let long = "一二三四五六七八九十一二三四";
    let keys = ["yi1"; 14].join("'");
    let lines = [
        b"ni3 \xe4\xbd\xa0 16777222 100\nni2'hao3 \xff 16777222 900\nni2 \xff 16777222 300\n"
            .as_slice(),
        format!("{keys} {long} 16870553 100\nyi1'yi1 一 16870554 100\n").as_bytes(),
    ]
    .concat();
    std::fs::write(&table, lines).unwrap();
    let name = oxpinyin_input::c_path(&table).unwrap();
    let mut file = TableFile::open(&name).unwrap();
    let mut libraries: [Option<ChunkMap>; 16] = Default::default();
    load_phrase_index(1, &mut file, &mut libraries, PhoneticType::Pinyin);
    let items: Vec<_> = libraries[1].take().unwrap().into_iter().collect();
    let frame = oxpinyin_data::chunk_write::build_chunk(&items).unwrap();
    oxpinyin_input::chunk::save(&chunk, &frame).unwrap();
    let library = PhraseLibrary::load_heap(&chunk).unwrap();
    let single = library.item(16777222).unwrap();
    assert_eq!(single.phrase_text().unwrap(), "你");
    assert_eq!(
        single
            .pronunciations()
            .map(|reading| reading.freq)
            .collect::<Vec<_>>(),
        [100, 300]
    );
    let item = library.item(16870553).unwrap();
    assert_eq!(item.phrase_text().unwrap(), long);
    assert_eq!(item.n_pronunciations(), 1);
    assert_eq!(library.item(16870554).unwrap().n_pronunciations(), 0);

    let config = dir.path().join("table.conf");
    std::fs::write(&config, format!("binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\ndefault GB_DICTIONARY gb.table {} gb.dbin SYSTEM_FILE\n", chunk.display())).unwrap();
    let conf = TableConf::load_from_dir(dir.path()).unwrap();
    let mut libraries = crate::storage::libraries::Libraries::load(&conf);
    libraries.add_unigram(16870553, 3);
    libraries.save(&conf);
    let library = PhraseLibrary::load_heap(&chunk).unwrap();
    assert_eq!(library.item(16870553).unwrap().unigram(), 3);
}
