use super::AddonDictionary;

pub(crate) fn open_dictionary_parts(
    system_dir: &std::path::Path,
) -> Result<(super::SystemTables, super::PhraseLibraries), super::DictError> {
    use super::{
        ChewingTable, DictError, PhraseLibraries, PhraseTable, SystemDbm, SystemTables, TableError,
    };
    use crate::data::{SYSTEM_LIBRARY_FILES, chewing_table::RawChewingDbm};
    use crate::store::{DefaultStore, RawReadStore};

    let open = |dbm: SystemDbm| {
        let path = crate::input::build_path(system_dir, std::ffi::OsStr::new(&dbm.file_name()));
        DefaultStore::open_read_only(&path).map_err(TableError::from)
    };
    let pinyin = ChewingTable::new(RawChewingDbm::new(open(SystemDbm::PinyinIndex)?));
    let phrase = PhraseTable::new(RawChewingDbm::new(open(SystemDbm::PhraseIndex)?));
    let tables = SystemTables::new(pinyin, phrase, 0);
    let libraries =
        PhraseLibraries::open(system_dir, SYSTEM_LIBRARY_FILES).map_err(DictError::Library)?;
    Ok((tables, libraries))
}

// 074a2219 pinyin.cpp:477-494 reaches the missing system filename at 284-290.
#[test]
fn addon_without_chunk_reports_null_chunk_name() {
    use crate::data::phrase_library::{LibraryError, RowFault};
    use std::path::Path;
    let mut addon = AddonDictionary::empty();
    addon.rows[4].system = None;
    assert!(matches!(
        addon.load_reporting(4, Path::new("unused")),
        Err(LibraryError::Row(RowFault::NullChunkName))
    ));
}

mod system_dictionary {
    use crate::vocab::PHRASE_INDEX_LIBRARY_COUNT;
    use crate::{
        model::{Dictionary, PhraseIndexRanges},
        parser::ChewingKey,
        vocab::SyllableKey,
    };

    use crate::data::row_format::pinyin_index::{
        encode_complete_key as encode_complete, encode_incomplete_key as encode_incomplete,
        encode_item,
    };

    use crate::data::{SystemDbm, SystemDictionary};

    use crate::store::DefaultStore;

    use crate::data::support::{ChunkBuilder, write_raw_rows};

    use tempfile::TempDir;

    struct Fixture {
        dir: TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = scratch_dir("oxpinyin-system-dict-test-");
            write_pinyin_index(&dir.path().join(SystemDbm::PinyinIndex.file_name()));
            write_raw_rows::<DefaultStore>(
                &dir.path().join(SystemDbm::PhraseIndex.file_name()),
                &[],
            );
            write_gb_char_chunk(&dir.path().join("gb_char.bin"));
            Self { dir }
        }

        fn open(&self) -> (crate::data::SystemTables, crate::data::PhraseLibraries) {
            super::open_dictionary_parts(self.dir.path()).unwrap()
        }
    }

    fn scratch_dir(prefix: &str) -> TempDir {
        tempfile::Builder::new().prefix(prefix).tempdir().unwrap()
    }

    fn key(spelling: &str) -> ChewingKey {
        ChewingKey::from_pinyin(spelling).unwrap()
    }

    fn encode_items(entries: &[(u32, &[ChewingKey])]) -> Vec<u8> {
        let mut buf = Vec::new();
        for (token, keys) in entries {
            buf.extend_from_slice(&encode_item(*token, keys));
        }
        buf
    }

    fn write_pinyin_index(path: &std::path::Path) {
        let ni3 = key("ni").with_tone(3);
        let ni2 = key("ni").with_tone(2);
        let hao3 = key("hao").with_tone(3);
        let rows = [
            (
                encode_complete(&[ni3]),
                encode_items(&[(0x0100_0010, &[ni3]), (0x0200_0010, &[ni2])]),
            ),
            (
                encode_complete(&[ni3, hao3]),
                encode_items(&[(0x0100_0099, &[ni3, hao3])]),
            ),
        ];
        write_raw_rows::<DefaultStore>(path, &rows);
    }

    fn write_gb_char_chunk(path: &std::path::Path) {
        let ni3 = key("ni").with_tone(3);
        let ni2 = key("ni").with_tone(2);
        let hao3 = key("hao").with_tone(3);

        let pron1 = |key: ChewingKey, freq: u32| (key.to_packed().to_le_bytes().to_vec(), freq);
        let pron2 = |a: ChewingKey, b: ChewingKey| {
            let mut keys = a.to_packed().to_le_bytes().to_vec();
            keys.extend_from_slice(&b.to_packed().to_le_bytes());
            vec![(keys, 5_u32)]
        };

        let mut builder = ChunkBuilder::new(10);
        builder.add(0x10, 5, "你", vec![pron1(ni3, 5), pron1(ni2, 3)]);
        builder.add(0x99, 5, "你好", pron2(ni3, hao3));
        std::fs::write(path, builder.build()).unwrap();
    }

    fn syl(text: &str) -> SyllableKey {
        SyllableKey::from_text(text).expect("frozen syllable")
    }

    fn syls(text: &str) -> Vec<SyllableKey> {
        text.split(',').map(syl).collect()
    }

    // Rust trellis_item prices the matched readings when a token enters the step.
    #[test]
    fn a_step_prices_the_pronunciation_possibility_of_its_token() {
        let fix = Fixture::new();
        let (tables, libraries) = fix.open();
        let dict = tables.read(&libraries);
        let entries = dict.lookup(&syls("ni")).unwrap();
        assert!(
            entries
                .iter()
                .all(|entry| entry.pronunciation_possibility().is_none())
        );
        let ni = entries[0].token();
        assert_eq!(
            dict.trellis_item(&syls("ni"), ni).unwrap().pronunciation,
            Some((8, 8))
        );
        let entries = dict.lookup(&syls("ni,hao")).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text(), "你好");
        let nihao = entries[0].token();
        assert_eq!(
            dict.trellis_item(&syls("ni,hao"), nihao)
                .unwrap()
                .pronunciation,
            Some((5, 5))
        );
    }

    // Rust phrase_prefix_exists accepts the empty prefix before probing the index.
    #[test]
    fn phrase_prefix_exists_follows_search_continued() {
        let fix = Fixture::new();
        let (tables, libraries) = fix.open();
        let dict = tables.read(&libraries);
        assert!(dict.phrase_prefix_exists(&[]));
    }

    // Rust SystemDictionary resolves token fields and renders readings through the selected library.
    #[test]
    fn token_surface_resolves_through_the_chunk_file() {
        let fix = Fixture::new();
        let (tables, libraries) = fix.open();
        let dict = tables.read(&libraries);
        assert_eq!(
            dict.phrase_text(0x0100_0099)
                .as_ref()
                .and_then(|text| text.as_str()),
            Some("你好")
        );
        assert_eq!(dict.phrase_text(0x0200_0010), None, "unloaded library");
        assert_eq!(dict.phrase_text(0xFFFF_FFFF), None);
        assert_eq!(dict.unigram_count(0x0100_0010), Some(5));
        let prons = dict.pronunciations(0x0100_0010);
        assert_eq!(prons.len(), 2);
        assert_eq!(prons[0], ("ni".to_owned(), 5));
        assert_eq!(prons[1], ("ni".to_owned(), 3));
    }

    // Rust The read-only fixture requires the phrase index.
    #[test]
    fn a_missing_dbm_fails_open() {
        let fix = Fixture::new();
        std::fs::remove_file(fix.dir.path().join(SystemDbm::PhraseIndex.file_name())).unwrap();
        assert!(super::open_dictionary_parts(fix.dir.path()).is_err());
    }

    fn convert_fixture() -> Fixture {
        let dir = scratch_dir("oxpinyin-system-dict-convert-");
        let ni = key("ni");
        let ni3 = ni.with_tone(3);
        let ni2 = ni.with_tone(2);
        let items = encode_items(&[
            (0x0100_0010, &[ni3][..]),
            (0x0200_0001, &[ni3][..]),
            (0x0100_0011, &[ni3][..]),
            (0x0100_0014, &[ni2][..]),
            (0x0100_0012, &[ni3][..]),
            (0x0100_0013, &[ni3][..]),
        ]);
        let n_key = ChewingKey::new(ni.initial, 0, 0, 0);
        write_raw_rows::<DefaultStore>(
            &dir.path().join(SystemDbm::PinyinIndex.file_name()),
            &[
                (encode_complete(&[ni]), items.clone()),
                (encode_complete(&[ni, ni]), Vec::new()),
                (encode_incomplete(&[n_key]), items),
            ],
        );
        write_raw_rows::<DefaultStore>(&dir.path().join(SystemDbm::PhraseIndex.file_name()), &[]);
        let pron = |key: ChewingKey| vec![(key.to_packed().to_le_bytes().to_vec(), 5_u32)];
        let mut builder = ChunkBuilder::new(20);
        builder.add(0x10, 5, "你", pron(ni3));
        builder.add(0x11, 5, "拟", pron(ni3));
        builder.add(0x12, 5, "逆", pron(ni3));
        builder.add(0x14, 5, "泥", pron(ni2));
        std::fs::write(dir.path().join("gb_char.bin"), builder.build()).unwrap();
        Fixture { dir }
    }

    type Spans = Vec<Vec<(u32, u32)>>;

    fn convert_ranges(dict: &SystemDictionary<'_>, tones: &[u8]) -> (Spans, Vec<String>) {
        let mut ranges = PhraseIndexRanges::new();
        dict.trellis_ranges(&syls("ni"), tones, &mut ranges)
            .unwrap();
        let spans = (0..usize::from(PHRASE_INDEX_LIBRARY_COUNT))
            .map(|library| {
                ranges
                    .ranges(library)
                    .iter()
                    .map(|range| (range.first(), range.last()))
                    .collect()
            })
            .collect();
        let texts = dict
            .trellis_records(&syls("ni"), tones)
            .unwrap()
            .iter()
            .filter_map(|entry| entry.text_str().map(str::to_owned))
            .collect();
        (spans, texts)
    }

    // 074a2219 pinyin.cpp:1635-1637,2053 requires a readable item for a loaded library.
    #[test]
    fn the_lookup_answers_that_it_met_an_unreadable_item() {
        let fix = convert_fixture();
        let (tables, libraries) = fix.open();
        let dict = tables.read(&libraries);
        let mut entries = Vec::new();
        assert!(dict.lookup_into_flagged(&syls("ni"), &mut entries).unwrap());
        let tokens: Vec<u32> = entries.iter().map(|entry| entry.token()).collect();
        assert_eq!(tokens, [0x0100_0010, 0x0100_0011, 0x0100_0014, 0x0100_0012]);
        assert_eq!(dict.lookup(&syls("ni")).unwrap(), entries);
    }

    // 074a2219 chewing_large_table2.h:91-150 forms token runs in filtered row order.
    #[test]
    fn trellis_ranges_merge_runs_past_unloaded_items_under_the_tone_filter() {
        let fix = convert_fixture();
        let (tables, libraries) = fix.open();
        let dict = tables.read(&libraries);
        for (tones, expected_spans, expected_texts) in [
            (
                &[3][..],
                &[(0x0100_0010, 0x0100_0013)][..],
                &["你", "拟", "逆"][..],
            ),
            (
                &[],
                &[
                    (0x0100_0010, 0x0100_0011),
                    (0x0100_0014, 0x0100_0014),
                    (0x0100_0012, 0x0100_0013),
                ],
                &["你", "拟", "泥", "逆"],
            ),
        ] {
            let (spans, texts) = convert_ranges(&dict, tones);
            assert_eq!(spans[1], expected_spans);
            assert!(spans[2].is_empty());
            assert_eq!(texts, expected_texts);
        }
    }

    // Rust trellis_ranges_continued returns the same ranges and presence as the separate probes.
    #[test]
    fn trellis_ranges_continued_matches_the_ranges_and_prefix_pair() {
        let fix = convert_fixture();
        let (tables, libraries) = fix.open();
        let dict = tables.read(&libraries);
        let mut scratch = Vec::new();
        for (path, tones) in [
            ("ni", &[3][..]),
            ("ni,ni", &[]),
            ("ni,hao", &[]),
            ("n", &[]),
        ] {
            let path = syls(path);
            let mut pair = PhraseIndexRanges::new();
            dict.trellis_ranges(&path, tones, &mut pair).unwrap();
            let continued = dict.phrase_prefix_exists(&path);
            let mut fused = PhraseIndexRanges::new();
            let fused_continued = dict
                .trellis_ranges_continued(&path, tones, &mut fused, &mut scratch)
                .unwrap();
            assert_eq!(
                format!("{pair:?}"),
                format!("{fused:?}"),
                "{path:?} {tones:?}"
            );
            assert_eq!(continued, fused_continued, "{path:?} {tones:?}");
        }
    }
}

#[cfg(unix)]
mod phrase_library {
    use crate::data::AddonDictionary;
    use crate::data::support::ChunkBuilder;

    fn sample() -> Vec<u8> {
        let mut builder = ChunkBuilder::new(5);
        builder.add(4, 5, "你", vec![(vec![0x12, 0x34], 5)]);
        builder.build()
    }

    #[cfg(unix)]
    // 074a2219 pinyin.cpp:477-494 checks the addon row and loaded state before opening its chunk.
    #[test]
    fn addon_load_keeps_its_answer_and_reports_the_refusal() {
        let dir = tempfile::tempdir().expect("scratch directory");
        let mut addons = AddonDictionary::empty();
        let error = addons
            .load_reporting(4, dir.path())
            .expect_err("art.bin is absent");
        assert_eq!(
            error.unmappable_path(),
            Some(dir.path().join("art.bin").as_path())
        );
        std::fs::write(dir.path().join("art.bin"), sample()).expect("write art.bin");
        assert!(matches!(addons.load_reporting(4, dir.path()), Ok(true)));
        std::fs::remove_file(dir.path().join("art.bin")).expect("remove art.bin");
        assert!(matches!(addons.load_reporting(4, dir.path()), Ok(false)));
        assert!(matches!(addons.load_reporting(3, dir.path()), Ok(false)));
    }
}
