use std::path::Path;

use super::{LibraryError, PhraseLibrary, RowFault, map_header};
use crate::chunk_write::{ChunkItem, build_chunk};

#[test]
fn heap_loaded_library_keeps_its_payload_after_file_changes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("library.bin");
    let item = ChunkItem {
        phrase: vec![0x4f60],
        unigram: 7,
        prons: vec![],
    };
    let bytes = build_chunk(&[(1, item)]).expect("chunk");
    std::fs::write(&path, bytes).expect("write");
    let library = PhraseLibrary::load_heap(&path).expect("load");
    std::fs::write(&path, []).expect("replace file");
    assert_eq!(library.item(1).map(|item| item.unigram()), Some(7));
    assert_eq!(library.total_freq(), 7);
}

// Pin memory_chunk.h:479-497: a failed header read follows the eight-byte gate.
#[test]
fn mapping_header_size_and_read_failure_follow_pin() {
    let path = Path::new("chunk");
    let calls = std::cell::Cell::new(0);
    let read = || {
        calls.set(calls.get() + 1);
        Err(std::io::ErrorKind::Other.into())
    };
    assert!(matches!(
        map_header(path, 7, read),
        Err(LibraryError::Unmappable { .. })
    ));
    assert_eq!(calls.get(), 0);
    assert!(matches!(
        map_header(path, 8, read),
        Err(LibraryError::Row(RowFault::ChunkHeaderRead))
    ));
    assert_eq!(calls.get(), 1);
    let mut reads = 0;
    assert!(matches!(
        map_header(path, 8, || {
            reads += 1;
            if reads == 1 {
                Ok([0; 4])
            } else {
                Err(std::io::ErrorKind::Other.into())
            }
        }),
        Err(LibraryError::Row(RowFault::ChunkHeaderRead))
    ));
    assert_eq!(reads, 2);
    assert!(map_header(path, 8, || Ok([0; 4])).is_ok());
}

#[test]
fn count_checks_offset_and_two_header_bytes_only() {
    let mut payload = Vec::new();
    let content = [vec![0; 8], vec![255, 17]].concat();
    let offsets = [0_u32, 8, 9, 10, 0];
    let index_two = 17 + offsets.len() as u32 * 4 + 1;
    for value in [0, 17, index_two, index_two + content.len() as u32 + 1] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.push(b'#');
    for offset in offsets {
        payload.extend_from_slice(&offset.to_le_bytes());
    }
    payload.push(b'#');
    payload.extend_from_slice(&content);
    payload.push(b'#');
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("counts.bin");
    std::fs::write(
        &path,
        crate::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write");
    let library = PhraseLibrary::open(&path).expect("open");
    assert_eq!(library.pronunciation_count(1), Some(17));
    assert!(library.item(1).is_none(), "the body is truncated");
    for token in [0, 2, 3, 4, 5, u32::MAX] {
        assert_eq!(library.pronunciation_count(token), None, "token {token}");
    }
}

#[cfg(all(test, unix, target_pointer_width = "64"))]
mod sparse_mapping_tests {
    use super::super::map_chunk;
    use std::io::Write;

    #[test]
    fn a_sparse_chunk_maps_the_whole_file() {
        // The pin maps the whole 2^32+8 file. Row 109 requires the crate
        // mapping to release the whole extent when it is dropped.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("merged.bin");
        let mut file = std::fs::File::create(&path).expect("create");
        file.set_len((1_u64 << 32) + 8).expect("sparse length");
        file.write_all(&[0_u8; 8]).expect("header");
        drop(file);
        let mapped = map_chunk(&path).expect("map");
        assert_eq!(
            mapped.as_slice().len(),
            (1_usize << 32) + 8,
            "the sparse mapping covers the whole file"
        );
    }
}

mod phrase_library {
    //! P1 reader tests: synthetic files in the exact upstream layout and the
    //! malformed-input refusals. The `package_libraries` tests below read the real files.
    //!
    //! The synthetic builder, `ChunkBuilder` in `tests/support/mod.rs`, is a
    //! faithful `SubPhraseIndex::store`
    //! counterpart (the seed of the P5 native emitter): chunk header with
    //! `chunk_format`'s checksum, the `'#'`-separated sections, and items
    //! written at `add_phrase_item`'s offsets (`FIRST_ITEM_OFFSET`, `0` = no
    //! item).

    // The chunk header size and checksum come from the crate's own shared
    // format module, as the builder's layout does — the fixtures are built
    // with the layout under test, not a second copy of it.
    use crate::chunk_format::{CHUNK_HEADER_SIZE, chunk_checksum};
    use crate::phrase_library::{PhraseLibrary, verify_chunk};
    use crate::support::{ChunkBuilder, write_temp};

    /// 你 (slot 4, one pronunciation of 2 packed keys) and 你好 (slot 9,
    /// two pronunciations of 4 packed keys) — the shapes the decode tests
    /// read back.
    fn sample() -> Vec<u8> {
        let mut builder = ChunkBuilder::new(9);
        builder.add(4, 5, "你", vec![(vec![0x12, 0x34], 5)]);
        builder.add(
            9,
            4,
            "你好",
            vec![
                (vec![0x12, 0x34, 0x56, 0x78], 3),
                (vec![0xAA, 0xBB, 0xCC, 0xDD], 1),
            ],
        );
        builder.build()
    }

    #[test]
    fn reads_items_texts_and_pronunciations() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let path = write_temp(scratch.path(), "sample", &sample());
        let library = PhraseLibrary::open(&path).expect("valid chunk");
        assert_eq!(library.total_freq(), 9);
        // Slots 1..=9 with trailing-zero trimming: slot 9 is the last item.
        assert_eq!(library.token_range(), 1..10);

        let item = library.item_at_slot(4).expect("item at slot 4");
        assert_eq!(item.phrase_length(), 1);
        assert_eq!(item.n_pronunciations(), 1);
        assert_eq!(item.unigram(), 5);
        assert_eq!(
            item.phrase_text().as_ref().and_then(|text| text.as_str()),
            Some("你")
        );
        let pronunciations: Vec<_> = item.pronunciations().collect();
        assert_eq!(pronunciations.len(), 1);
        assert_eq!(pronunciations[0].keys, &[0x12, 0x34]);
        assert_eq!(pronunciations[0].freq, 5);

        let item = library.item_at_slot(9).expect("item at slot 9");
        assert_eq!(
            item.phrase_text().as_ref().and_then(|text| text.as_str()),
            Some("你好")
        );
        let pronunciations: Vec<_> = item.pronunciations().collect();
        assert_eq!(pronunciations.len(), 2);
        assert_eq!(pronunciations[1].keys, &[0xAA, 0xBB, 0xCC, 0xDD]);
        assert_eq!(pronunciations[1].freq, 1);
        assert!(item.pronunciation(2).is_none());

        // Slot addressing through a full token masks the library nibble.
        assert!(library.item(0x0100_0004).is_some());
        assert!(library.item(0x0F00_0004).is_some());

        // Absent and out-of-range slots answer None, never panic.
        assert!(library.item_at_slot(0).is_none());
        assert!(library.item_at_slot(5).is_none());
        assert!(library.item_at_slot(10).is_none());
        assert!(library.item_at_slot(usize::MAX / 4).is_none());

        // The tooling iteration sees exactly the two items.
        assert_eq!(library.items().count(), 2);
    }

    #[test]
    fn sub_index_layout_damage_is_refused() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        // `chunk_level_refusals_name_the_file_and_layout_faults_do_not` holds the
        // chunk header refusals and the header separator.
        let sample = sample();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("offset-array separator lost", {
                let mut bytes = sample.clone();
                let index_two = u32_at(&sample[16..20]);
                bytes[8 + index_two as usize - 1] = b'!';
                fix_checksum(&mut bytes);
                bytes
            }),
            ("final separator lost", {
                let mut bytes = sample.clone();
                let last = bytes.len() - 1;
                bytes[last] = b'!';
                fix_checksum(&mut bytes);
                bytes
            }),
            ("sections out of order", {
                let mut bytes = sample.clone();
                bytes[8 + 12..8 + 16].copy_from_slice(&2_u32.to_le_bytes());
                fix_checksum(&mut bytes);
                bytes
            }),
            ("offset array not u32 sized", {
                // index_two one byte past where the separator check
                // still passes is not constructible without breaking a
                // separator; emulate via index_one drift instead.
                let mut bytes = sample.clone();
                bytes[8 + 4..8 + 8].copy_from_slice(&18_u32.to_le_bytes());
                fix_checksum(&mut bytes);
                bytes
            }),
        ];
        for (name, bytes) in cases {
            let path = write_temp(scratch.path(), name, &bytes);
            let result = PhraseLibrary::open(&path);
            assert!(result.is_err(), "{name} must be refused, got {result:?}");
        }
    }

    #[test]
    fn adversarial_inputs_never_overflow() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        // Two shapes found in review, now refused/None at every opt level:
        //
        // 1. index_two == index_one (no room even for the offset array's
        //    separator) used to underflow `index_two - index_one - 1`.
        let sample = sample();
        let mut collapsed = sample.clone();
        let index_two = u32_at(&collapsed[16..20]);
        collapsed[12..16].copy_from_slice(&index_two.to_le_bytes()); // index_three := index_two
        // Make index_two equal index_one (17): shift the words.
        collapsed[8..12].copy_from_slice(&17_u32.to_le_bytes());
        fix_checksum(&mut collapsed);
        let path = write_temp(scratch.path(), "collapsed", &collapsed);
        assert!(PhraseLibrary::open(&path).is_err());

        // 2. A slot whose byte offset overflows usize on multiply.
        let path = write_temp(scratch.path(), "sample-again", &sample);
        let library = PhraseLibrary::open(&path).expect("valid chunk");
        assert!(library.item_at_slot(usize::MAX / 2).is_none());
        assert!(library.item_at_slot(usize::MAX).is_none());
    }

    #[test]
    fn item_damage_answers_no_item_never_panics() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        // Point slot 4's offset at the last bytes of the entry area, where
        // an item header cannot fit: bounded reads, None answers.
        let mut bytes = sample();
        let payload_len = bytes.len() - 8;
        let index_two = u32_at(&bytes[16..20]);
        let content_start = 8 + index_two as usize;
        let near_end = u32::try_from(8 + payload_len - 2 - content_start) // content-relative
            .expect("near_end fits u32");
        bytes[8 + 17 + 4 * 4..8 + 17 + 4 * 4 + 4].copy_from_slice(&near_end.to_le_bytes());
        fix_checksum(&mut bytes);
        let path = write_temp(scratch.path(), "damaged-item", &bytes);
        let library = PhraseLibrary::open(&path).expect("layout still validates");
        assert!(
            library.item_at_slot(4).is_none(),
            "malformed item is no item"
        );

        // An invalid UCS-4 scalar in the text degrades to None text.
        let mut builder = ChunkBuilder::new(1);
        builder.add(1, 1, "\u{1}\u{2}", vec![(vec![0; 4], 1)]);
        let mut bytes = builder.build();
        // Overwrite the second char's scalar with a surrogate.
        let index_two = u32_at(&bytes[16..20]);
        let content_start = 8 + index_two as usize;
        let text_start = content_start + 8 + 6; // reserved 8 + item header 6
        bytes[text_start + 4..text_start + 8].copy_from_slice(&0xD800_u32.to_le_bytes());
        fix_checksum(&mut bytes);
        let path = write_temp(scratch.path(), "surrogate", &bytes);
        let library = PhraseLibrary::open(&path).expect("layout still validates");
        let item = library.item_at_slot(1).expect("item present");
        assert_eq!(item.phrase_length(), 2);
        let text = item.phrase_text().expect("surrogate is stored");
        assert_eq!(text.stored_words(), Some([0x01, 0xD800].as_slice()));
        assert!(text.as_str().is_none(), "surrogate is not a char");
    }

    #[test]
    fn an_embedded_zero_ends_the_phrase_text() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let mut builder = ChunkBuilder::new(1);
        builder.add(1, 1, "\u{1}\u{0}\u{2}", vec![(vec![0; 6], 1)]);
        let bytes = builder.build();
        let path = write_temp(scratch.path(), "nul-text", &bytes);
        let library = PhraseLibrary::open(&path).expect("layout validates");
        let item = library.item_at_slot(1).expect("item present");
        assert_eq!(item.phrase_length(), 3);
        let text = item.phrase_text().expect("zero is stored, not refused");
        assert_eq!(text.stored_words(), Some([0x01, 0, 0x02].as_slice()));
        assert!(text.as_str().is_none(), "the stored form carries the cut");
    }

    fn u32_at(bytes: &[u8]) -> u32 {
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    fn fix_checksum(bytes: &mut [u8]) {
        let sum = chunk_checksum(&bytes[CHUNK_HEADER_SIZE..]);
        bytes[4..CHUNK_HEADER_SIZE].copy_from_slice(&sum.to_le_bytes());
    }

    // ── the files the pin's `MemoryChunk::mmap` refuses (#545) ──────────

    /// `MemoryChunk::mmap` on a directory (`memory_chunk.h:479-493`): the size
    /// `lseek(SEEK_END)` reports decides between the ordinary refusal (under the
    /// 8-byte header) and the `read` failure the pin asserts on. The size is the
    /// file system's, so the test reads it the way the pin does and expects the
    /// matching outcome, whatever the temporary directory's file system is.
    // Unix only: a directory opens as a file, so `lseek(SEEK_END)` reports its size.
    #[cfg(unix)]
    #[test]
    fn a_directory_chunk_follows_the_size_the_file_system_reports() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        use crate::phrase_library::{LibraryError, RowFault};
        use std::io::{Seek, SeekFrom};
        for (name, entry) in [("long", "e".repeat(25)), ("short", "x".to_owned())] {
            let path = scratch.path().join(format!("directory-chunk-{name}"));
            std::fs::create_dir(&path).expect("create temp dir");
            std::fs::write(path.join(&entry), b"x").expect("fill the directory");
            let size = std::fs::File::open(&path).and_then(|mut dir| dir.seek(SeekFrom::End(0)));
            for error in [
                PhraseLibrary::open(&path).expect_err(name),
                verify_chunk(&path).expect_err(name),
            ] {
                match &size {
                    Err(seek_error) => {
                        assert_eq!(error.unmappable_path(), Some(path.as_path()), "{name}");
                        let LibraryError::Unmappable { cause, .. } = &error else {
                            panic!("{name}: {error}");
                        };
                        let LibraryError::Io(io_error) = cause.as_ref() else {
                            panic!("{name}: {error}");
                        };
                        assert_eq!(io_error.raw_os_error(), seek_error.raw_os_error(), "{name}");
                    }
                    Ok(size) if *size >= 8 => {
                        assert!(
                            matches!(error, LibraryError::Row(RowFault::ChunkHeaderRead)),
                            "{name}: size {size}: {error}"
                        );
                        assert!(error.unmappable_path().is_none(), "{name}");
                    }
                    Ok(size) => {
                        assert_eq!(
                            error.unmappable_path(),
                            Some(path.as_path()),
                            "{name}: {size}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn chunk_level_refusals_name_the_file_and_layout_faults_do_not() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let sample = sample();
        // `MemoryChunk::mmap` answers false: no bytes at all, a header cut short,
        // a length word that is not the payload length, a bad checksum.
        let chunk_faults: Vec<(&str, Vec<u8>)> = vec![
            ("empty", Vec::new()),
            ("short header", sample[..7].to_vec()),
            ("payload truncation", sample[..sample.len() - 1].to_vec()),
            ("checksum drift", {
                let mut bytes = sample.clone();
                bytes[4] ^= 0xFF;
                bytes
            }),
        ];
        for (name, bytes) in chunk_faults {
            let path = write_temp(scratch.path(), name, &bytes);
            let error = PhraseLibrary::open(&path).expect_err(name);
            assert_eq!(error.unmappable_path(), Some(path.as_path()), "{name}");
            let error = verify_chunk(&path).expect_err(name);
            assert_eq!(error.unmappable_path(), Some(path.as_path()), "{name}");
        }
        // No file: the open fails, which is the same refusal.
        let absent = scratch.path().join("absent.bin");
        let error = PhraseLibrary::open(&absent).expect_err("absent file");
        assert_eq!(error.unmappable_path(), Some(absent.as_path()));

        // A chunk that maps but whose sub-index layout `SubPhraseIndex::load`
        // refuses is the pin's `g_return_val_if_fail`, not an `mmap` line.
        let mut bytes = sample;
        bytes[8 + 16] = b'!';
        fix_checksum(&mut bytes);
        let path = write_temp(scratch.path(), "layout", &bytes);
        let error = PhraseLibrary::open(&path).expect_err("layout fault");
        assert!(error.unmappable_path().is_none(), "{error}");
        verify_chunk(&path).expect("the chunk itself maps");
    }
}

#[cfg(feature = "tkrzw")]
#[path = "../../../oxpinyin-facade/tests/common/contract_data.rs"]
mod contract_data;

#[cfg(feature = "tkrzw")]
mod package_libraries {
    //! The phrase-library reader over the chunk files of the pinned libpinyin-data
    //! package (`just fetch-contract-data`). The package's other files are in tkrzw
    //! format, so this module requires that backend.

    use super::contract_data;
    use crate::phrase_library::PhraseLibrary;

    #[test]
    fn reads_the_pinned_package_libraries() {
        let dir = contract_data::contract_data_dir();

        let mut libraries = Vec::new();
        let mut items = 0_u64;
        for name in ["gb_char", "gbk_char", "opengram", "merged"] {
            let library = PhraseLibrary::open(&dir.join(format!("{name}.bin")))
                .unwrap_or_else(|e| panic!("{name}.bin: {e}"));
            assert!(library.total_freq() > 0, "{name} carries unigram mass");
            assert_eq!(library.token_range().start, 1, "{name} tokens start at 1");
            let count = library.items().count() as u64;
            assert!(count > 0, "{name} has items");
            items += count;
            libraries.push(library);
        }
        // The four system libraries of the model libpinyin 074a2219 builds: 138,096
        // tokens.
        assert_eq!(items, 138_096);

        // 的: token 0x010005DB. The pinned model's `gb_char.table` carries two
        // readings, `de 的 16778715 2213855` and `di 的 16778715 11000`, the counts
        // the chunk stores verbatim.
        let gb = &libraries[0];
        let item = gb.item(0x0100_05DB).expect("的 present in gb_char");
        assert_eq!(
            item.phrase_text().as_ref().and_then(|text| text.as_str()),
            Some("的")
        );
        let mut freqs: Vec<u32> = item
            .pronunciations()
            .map(|pronunciation| pronunciation.freq)
            .collect();
        freqs.sort_unstable();
        assert_eq!(freqs, [11_000, 2_213_855]);

        // 锕: token 0x01000001, one reading, `a 锕 … 7` in the same table.
        let item = gb.item(0x0100_0001).expect("锕 present in gb_char");
        assert_eq!(
            item.phrase_text().as_ref().and_then(|text| text.as_str()),
            Some("锕")
        );
        assert_eq!(item.n_pronunciations(), 1);
        assert_eq!(
            item.pronunciations()
                .next()
                .map(|pronunciation| pronunciation.freq),
            Some(7)
        );

        // The addon chunk files ride the same format.
        let art = PhraseLibrary::open(&dir.join("art.bin")).expect("addon art.bin");
        assert!(art.items().count() > 0);
    }
}

/// Header and first fields stay readable past an unreadable second reading
/// (phrase_index.cpp:180-205 at 074a2219; compatibility policy row 130).
#[test]
fn a_shipped_item_with_a_reading_past_the_content_end_keeps_its_fields() {
    let scratch = tempfile::tempdir().unwrap();
    let system = crate::native_fixture::truncated_second_reading_system(scratch.path());
    let library = PhraseLibrary::open(&system.join("gb_char.bin")).unwrap();
    let item = library.item_fields(0x0100_0001).unwrap();
    assert_eq!(item.unigram(), 100);
    assert_eq!(item.phrase_text(), Some("丙".into()));
    assert_eq!(
        item.pronunciation(0).unwrap().keys,
        0x1081_u16.to_le_bytes()
    );
    assert!(item.pronunciation(1).is_none());
}
