use super::*;

// 074a2219 pinyin_internal.h:55-66 defines the native database filenames.
#[test]
fn names_are_libpinyins_own() {
    assert_eq!(SystemDbm::Bigram.file_name(), "bigram.db");
    assert_eq!(SystemDbm::PinyinIndex.file_name(), "pinyin_index.bin");
}

// 074a2219 data/table.conf.in:8-28 pairs each library stem with its chunk file.
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
