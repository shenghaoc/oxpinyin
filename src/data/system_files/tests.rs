use super::*;

#[test]
fn names_are_libpinyins_own() {
    assert_eq!(SystemDbm::Bigram.file_name(), "bigram.db");
    assert_eq!(SystemDbm::PinyinIndex.file_name(), "pinyin_index.bin");
}

/// The chunk-file tables are the name tables with `.bin` appended.
/// `gen_binary_files` compiles `<name>.table` into the chunk file this
/// crate opens, so a library added to one table and not the other
/// would give the two halves different library sets.
#[test]
fn chunk_file_tables_are_the_name_tables_plus_bin() {
    for (names, files) in [
        (SYSTEM_LIBRARY_NAMES, SYSTEM_LIBRARY_FILES),
        (ADDON_LIBRARY_NAMES, ADDON_LIBRARY_FILES),
    ] {
        assert_eq!(names.len(), files.len());
        for (&(name_nibble, name), &(file_nibble, file)) in names.iter().zip(files) {
            assert_eq!(name_nibble, file_nibble);
            assert_eq!(file, format!("{name}.bin"));
        }
    }
}
