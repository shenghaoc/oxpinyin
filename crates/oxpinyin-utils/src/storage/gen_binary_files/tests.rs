use super::*;
use oxpinyin_data::phrase_library::PhraseLibrary;
use std::{ffi::OsStr, path::Path};

fn open_table(path: &Path) -> TableFile {
    let name = oxpinyin_input::c_path(path).unwrap();
    TableFile::open(&name).unwrap()
}

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
    let mut file = open_table(&table);
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

/// Strict item loading refuses a consumed invalid phrase (policy row 115).
/// The new-item paths are at 074a2219 src/storage/phrase_index.cpp:541-556.
#[test]
fn invalid_text_for_a_new_item_is_refused_at_the_table_reader() {
    const NAME: &str = "storage::gen_binary_files::tests::invalid_text_for_a_new_item_is_refused_at_the_table_reader";
    const INPUT: &str = "OXPINYIN_INVALID_PHRASE_TABLE";
    const INDEX: &str = "OXPINYIN_INVALID_PHRASE_INDEX";
    if let Some(path) = std::env::var_os(INPUT) {
        let index = std::env::var(INDEX).unwrap().parse().unwrap();
        let mut file = open_table(Path::new(&path));
        let _ = read_phrase_items(index, &mut file, PhoneticType::Pinyin, true);
        panic!("invalid phrase was accepted");
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gb.table");
    for (index, bytes) in [
        ("1", b"ni3 \xff 16777222 100\n".as_slice()),
        (
            "1",
            b"ni3 \xe4\xbd\xa0 16777222 100\nni2 \xff 16777223 300\n".as_slice(),
        ),
        ("0", b"ni3 \xe4\xbd\xa0 0 100\nni2 \xff 0 300\n".as_slice()),
    ] {
        std::fs::write(&path, bytes).unwrap();
        let output = crate::test_child::run_exact(
            NAME,
            [(INPUT, path.as_os_str()), (INDEX, OsStr::new(index))],
        );
        assert_eq!(output.status.code(), Some(tool::EINVAL), "{output:?}");
        assert!(output.stderr.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

/// Strict table loading refuses the failed conversion in
/// 074a2219 src/storage/phrase_large_table3.cpp:125-139 (policy row 115).
#[cfg(unix)]
#[test]
fn phrase_table_invalid_text() {
    const NAME: &str = "storage::gen_binary_files::tests::phrase_table_invalid_text";
    const INPUT: &str = "OXPINYIN_INVALID_PHRASE_TABLE_ROW";
    const ROW: &[u8] = b"ni3 \xff 16777217 100\n";
    if let Some(path) = std::env::var_os(INPUT) {
        let table = Tree::memory().unwrap();
        let mut file = open_table(Path::new(&path));
        load_phrase_table(Some(&table), &mut file, true);
        panic!("invalid phrase was accepted");
    }

    let dir = assert_fs::TempDir::new().unwrap();
    let path = dir.path().join("gb.table");
    std::fs::write(&path, ROW).unwrap();
    let output = crate::test_child::run_exact(NAME, [(INPUT, &path)]);
    assert_eq!(output.status.code(), Some(tool::EINVAL), "{output:?}");
    assert!(output.stderr.is_empty());
    let harness = format!("\nrunning 1 test\ntest {NAME} ... ");
    assert_eq!(output.stdout, harness.as_bytes());
    assert_eq!(std::fs::read(&path).unwrap(), ROW);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
